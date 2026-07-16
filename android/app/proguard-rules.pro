# Application release shrinker configuration.
# Native method names are part of the Rust JNI ABI and must remain stable.
-keepclasseswithmembernames,includedescriptorclasses class * {
    native <methods>;
}

-keep class ai.mukei.android.core.nativebridge.NativeBindings { *; }
-keep class ai.mukei.android.core.nativebridge.RustNativeGateway { *; }
-keep class ai.mukei.android.core.nativebridge.SecureRuntimeFactory { *; }

# Keep Protocol V2 DTO metadata readable in crash-free release diagnostics.
-keepattributes RuntimeVisibleAnnotations,RuntimeInvisibleAnnotations,AnnotationDefault,InnerClasses,EnclosingMethod

# Normalize source names without preserving local source paths.
-renamesourcefileattribute SourceFile
