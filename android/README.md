# Mukei Android scaffold

This directory is the native Android application for the `Kotlin` production branch.

## Modules

- `app`: Compose application shell.
- `core:protocol`: Kotlin projection of the shared Protocol V2 boundary.
- `core:native`: added by the bridge scaffold and responsible for the narrow JNI gateway.

## Toolchain

- JDK 17
- Gradle 9.4.1
- Android Gradle Plugin 9.2.1
- Kotlin 2.4.10
- compile/target SDK 37
- minimum SDK 26

The repository currently uses CI-provisioned Gradle. A checked-in Gradle wrapper will be generated after the first verified Android build.

## Build

```bash
gradle -p android :app:assembleDebug
```
