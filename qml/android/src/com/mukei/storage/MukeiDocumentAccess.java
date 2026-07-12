package com.mukei.storage;

import android.app.Activity;
import android.content.ContentResolver;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.provider.OpenableColumns;

import org.qtproject.qt.android.QtNative;

/**
 * Android Storage Access Framework boundary for Mukei.
 *
 * The helper never returns filesystem paths. It only validates the selected
 * content URI, attempts to retain read permission, exposes bounded metadata,
 * and releases the retained grant during document revoke.
 */
public final class MukeiDocumentAccess {
    public static final int ACCESS_FAILED = -1;
    public static final int ACCESS_TRANSIENT = 0;
    public static final int ACCESS_PERSISTED = 1;
    public static final int ACCESS_NOT_REQUIRED = 2;

    private MukeiDocumentAccess() {}

    private static Activity activity() {
        Activity activity = QtNative.activity();
        if (activity == null) throw new IllegalStateException("Qt activity unavailable");
        return activity;
    }

    private static Uri parse(String value) {
        if (value == null || value.length() == 0 || value.length() > 16 * 1024) {
            throw new IllegalArgumentException("invalid document URI");
        }
        Uri uri = Uri.parse(value);
        String scheme = uri.getScheme();
        if (!"content".equals(scheme) && !"file".equals(scheme)) {
            throw new IllegalArgumentException("unsupported document URI scheme");
        }
        return uri;
    }

    private static boolean readable(ContentResolver resolver, Uri uri) {
        try (java.io.InputStream input = resolver.openInputStream(uri)) {
            return input != null;
        } catch (Throwable ignored) {
            return false;
        }
    }

    /**
     * Attempts to persist a read-only content URI grant.
     *
     * Some providers grant only transient access. In that case the method
     * returns ACCESS_TRANSIENT only after confirming that the URI is readable
     * for the current process. The Rust layer persists this distinction and
     * never claims durable access when the provider did not grant it.
     */
    public static int persistReadPermission(String value) {
        try {
            Uri uri = parse(value);
            if ("file".equals(uri.getScheme())) return ACCESS_NOT_REQUIRED;
            ContentResolver resolver = activity().getContentResolver();
            try {
                resolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
                return ACCESS_PERSISTED;
            } catch (SecurityException | UnsupportedOperationException notPersistable) {
                return readable(resolver, uri) ? ACCESS_TRANSIENT : ACCESS_FAILED;
            }
        } catch (Throwable ignored) {
            return ACCESS_FAILED;
        }
    }

    public static boolean releaseReadPermission(String value) {
        try {
            Uri uri = parse(value);
            if ("file".equals(uri.getScheme())) return true;
            activity().getContentResolver().releasePersistableUriPermission(
                    uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            return true;
        } catch (SecurityException noPersistedGrant) {
            // A transient grant has nothing durable to release.
            return true;
        } catch (Throwable ignored) {
            return false;
        }
    }

    public static boolean canRead(String value) {
        try {
            Uri uri = parse(value);
            if ("file".equals(uri.getScheme())) {
                java.io.File file = new java.io.File(uri.getPath());
                return file.isFile() && file.canRead();
            }
            return readable(activity().getContentResolver(), uri);
        } catch (Throwable ignored) {
            return false;
        }
    }

    public static long sizeBytes(String value) {
        try {
            Uri uri = parse(value);
            if ("file".equals(uri.getScheme())) {
                java.io.File file = new java.io.File(uri.getPath());
                return file.isFile() ? Math.max(0L, file.length()) : -1L;
            }
            Cursor cursor = activity().getContentResolver().query(
                    uri, new String[] { OpenableColumns.SIZE }, null, null, null);
            if (cursor == null) return -1L;
            try (Cursor closeable = cursor) {
                if (!closeable.moveToFirst()) return -1L;
                int index = closeable.getColumnIndex(OpenableColumns.SIZE);
                return index >= 0 && !closeable.isNull(index)
                        ? Math.max(0L, closeable.getLong(index)) : -1L;
            }
        } catch (Throwable ignored) {
            return -1L;
        }
    }

    public static String mimeType(String value) {
        try {
            Uri uri = parse(value);
            if ("file".equals(uri.getScheme())) return "application/octet-stream";
            String mime = activity().getContentResolver().getType(uri);
            return mime == null ? "application/octet-stream" : mime;
        } catch (Throwable ignored) {
            return "application/octet-stream";
        }
    }

    public static String displayName(String value) {
        try {
            Uri uri = parse(value);
            if ("file".equals(uri.getScheme())) {
                String segment = uri.getLastPathSegment();
                return segment == null ? "Private document" : segment;
            }
            Cursor cursor = activity().getContentResolver().query(
                    uri, new String[] { OpenableColumns.DISPLAY_NAME }, null, null, null);
            if (cursor == null) return "Private document";
            try (Cursor closeable = cursor) {
                if (!closeable.moveToFirst()) return "Private document";
                int index = closeable.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                String name = index >= 0 ? closeable.getString(index) : null;
                return name == null || name.length() == 0 ? "Private document" : name;
            }
        } catch (Throwable ignored) {
            return "Private document";
        }
    }
}
