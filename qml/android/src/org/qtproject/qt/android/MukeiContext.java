package org.qtproject.qt.android;

import android.content.Context;

/**
 * Small compatibility boundary for Mukei's Java helpers.
 *
 * Qt 6.5 exposed QtNative.activity() publicly, while Qt 6.8 keeps its activity
 * and context accessors package-private. Keeping this adapter in Qt's Android
 * package lets the app consume the current Context without depending on an
 * Activity-only API. The build therefore verifies compatibility against the
 * selected Qt Android jar at compile time.
 */
public final class MukeiContext {
    private MukeiContext() {}

    public static Context requireContext() {
        Context context = QtNative.getContext();
        if (context == null) {
            throw new IllegalStateException("Qt Android context unavailable");
        }
        Context applicationContext = context.getApplicationContext();
        return applicationContext != null ? applicationContext : context;
    }
}
