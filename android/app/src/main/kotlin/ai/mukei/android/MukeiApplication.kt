package ai.mukei.android

import android.app.Application

class MukeiApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        BackendRuntimeHost.start(this)
    }

    override fun onTerminate() {
        BackendRuntimeHost.shutdown()
        super.onTerminate()
    }
}
