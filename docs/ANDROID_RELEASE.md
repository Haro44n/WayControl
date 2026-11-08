# Android release signing

Release builds remain unsigned when no signing variables are set, which is suitable for local and CI build verification. To sign a release APK, provide a release keystore through all four environment variables before running `./gradlew assembleRelease` from `android/`:

- `WAYCONTROL_RELEASE_STORE_FILE`: path to the keystore
- `WAYCONTROL_RELEASE_STORE_PASSWORD`: keystore password
- `WAYCONTROL_RELEASE_KEY_ALIAS`: alias of the release key
- `WAYCONTROL_RELEASE_KEY_PASSWORD`: key password

If any variable is set, Gradle requires all four. Keep the keystore and passwords outside the repository and supply the variables through the build host's secret store. The release APK is written under `android/app/build/outputs/apk/release/`.

Verify a signed APK with the Android SDK's `apksigner verify --verbose --print-certs <apk>` before distributing it. The project does not generate, store, or select a release key.
