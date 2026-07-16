# JNI entry points use name-based symbol resolution from libmukei_android.so.
-keep class ai.mukei.android.core.nativebridge.NativeBindings { *; }
-keepclasseswithmembernames,includedescriptorclasses class * {
    native <methods>;
}

# Public gateway and platform processor form the library's Kotlin ABI.
-keep public class ai.mukei.android.core.nativebridge.RustNativeGateway { public *; }
-keep public class ai.mukei.android.core.nativebridge.AndroidPlatformRequestProcessor { public *; }
-keep public class ai.mukei.android.core.nativebridge.SecureRuntimeFactory { public *; }
