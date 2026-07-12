package com.mukei.security;

import android.app.Activity;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;

import org.qtproject.qt.android.QtNative;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.security.KeyStore;
import java.security.SecureRandom;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.Arrays;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/** App-private provider secret storage using a non-exportable AndroidKeyStore key. */
public final class MukeiSecretStore {
    private static final String KEYSTORE = "AndroidKeyStore";
    private static final String WRAP_ALIAS = "mukei_provider_wrap_v1";
    private static final int VERSION = 1;
    private static final int IV_BYTES = 12;
    private static final int MAX_SECRET_BYTES = 16 * 1024;

    private MukeiSecretStore() {}

    private static File directory() throws Exception {
        Activity activity = QtNative.activity();
        if (activity == null) throw new IllegalStateException("Qt activity unavailable");
        File directory = new File(activity.getFilesDir(), "secrets");
        if (!directory.exists() && !directory.mkdirs()) {
            throw new IllegalStateException("cannot create app-private secret directory");
        }
        return directory;
    }

    private static String safeAlias(String alias) {
        if (alias == null || !alias.matches("[A-Za-z0-9_.-]{1,64}")) {
            throw new IllegalArgumentException("invalid secret alias");
        }
        return alias;
    }

    private static SecretKey key() throws Exception {
        KeyStore store = KeyStore.getInstance(KEYSTORE);
        store.load(null);
        KeyStore.Entry entry = store.getEntry(WRAP_ALIAS, null);
        if (entry instanceof KeyStore.SecretKeyEntry) {
            return ((KeyStore.SecretKeyEntry) entry).getSecretKey();
        }
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE);
        generator.init(new KeyGenParameterSpec.Builder(
                WRAP_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .setKeySize(256)
                .build());
        return generator.generateKey();
    }

    public static boolean store(String alias, byte[] plaintext) {
        if (plaintext == null || plaintext.length == 0 || plaintext.length > MAX_SECRET_BYTES) return false;
        try {
            alias = safeAlias(alias);
            byte[] iv = new byte[IV_BYTES];
            new SecureRandom().nextBytes(iv);
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, key(), new GCMParameterSpec(128, iv));
            byte[] ciphertext = cipher.doFinal(plaintext);

            File destination = new File(directory(), alias + ".enc");
            File temporary = new File(directory(), alias + ".enc.tmp");
            try (FileOutputStream output = new FileOutputStream(temporary, false)) {
                output.write(VERSION);
                output.write(iv.length);
                output.write(iv);
                output.write(ciphertext);
                output.flush();
                output.getFD().sync();
            }
            try {
                Files.move(
                        temporary.toPath(),
                        destination.toPath(),
                        StandardCopyOption.ATOMIC_MOVE,
                        StandardCopyOption.REPLACE_EXISTING);
            } catch (java.nio.file.AtomicMoveNotSupportedException unsupported) {
                Files.move(
                        temporary.toPath(),
                        destination.toPath(),
                        StandardCopyOption.REPLACE_EXISTING);
            }
            return true;
        } catch (Throwable ignored) {
            return false;
        } finally {
            Arrays.fill(plaintext, (byte) 0);
        }
    }

    public static byte[] load(String alias) {
        try {
            alias = safeAlias(alias);
            File source = new File(directory(), alias + ".enc");
            if (!source.exists() || source.length() > MAX_SECRET_BYTES + 64L) return null;
            ByteArrayOutputStream bytes = new ByteArrayOutputStream();
            try (FileInputStream input = new FileInputStream(source)) {
                byte[] buffer = new byte[4096];
                int read;
                while ((read = input.read(buffer)) != -1) bytes.write(buffer, 0, read);
            }
            byte[] payload = bytes.toByteArray();
            if (payload.length < 2 + IV_BYTES || (payload[0] & 0xff) != VERSION) return null;
            int ivLength = payload[1] & 0xff;
            if (ivLength != IV_BYTES || payload.length <= 2 + ivLength) return null;
            byte[] iv = new byte[ivLength];
            System.arraycopy(payload, 2, iv, 0, ivLength);
            byte[] ciphertext = new byte[payload.length - 2 - ivLength];
            System.arraycopy(payload, 2 + ivLength, ciphertext, 0, ciphertext.length);
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.DECRYPT_MODE, key(), new GCMParameterSpec(128, iv));
            return cipher.doFinal(ciphertext);
        } catch (Throwable ignored) {
            return null;
        }
    }

    public static boolean delete(String alias) {
        try {
            alias = safeAlias(alias);
            File destination = new File(directory(), alias + ".enc");
            File temporary = new File(directory(), alias + ".enc.tmp");
            boolean ok = !destination.exists() || destination.delete();
            if (temporary.exists()) ok &= temporary.delete();
            return ok;
        } catch (Throwable ignored) {
            return false;
        }
    }
}
