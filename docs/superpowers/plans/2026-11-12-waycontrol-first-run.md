# WayControl First-Run Connection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax.

**Goal:** Guide a fresh installation from welcome through discovery and trusted pairing to the Home dashboard.

**Architecture:** Keep `SecureConnectionStore`, `WayControlClient`, and current Settings pairing form as the connection source of truth. Add a persisted first-run route decision and render a welcome presentation that reuses the existing discovery, QR, manual pairing, and approval flow.

**Tech Stack:** Kotlin, Jetpack Compose, AndroidX lifecycle, existing encrypted credential store and Google Play Code Scanner.

**Spec:** `docs/superpowers/specs/2026-11-12-waycontrol-first-run-design.md`

## Global Constraints

- Keep TLS certificate pinning, Linux-side approval, encrypted credential persistence, and explicit connection state.
- Keep discovery bounded and untrusted; it may fill only the endpoint.
- Preserve the saved-credential reconnect behavior and keep manual pairing available.
- Add no dependencies or persistent background service.

## Review Focus

- Credential restore must resolve before selecting the first-run route; test that saved credentials never flash the welcome screen.
- Skipping onboarding must persist across process recreation while keeping pairing available in Settings.
- A failed or pending pair must not mark onboarding complete or navigate Home.
- A confirmed approved token must be saved before onboarding exits.
- Discovery failure/no results and QR scanner failure must leave manual pairing usable.

---

### Task 1: First-run route state

**Files:**
- Create: `android/app/src/main/java/dev/waycontrol/FirstRunExperience.kt`
- Test: `android/app/src/test/java/dev/waycontrol/FirstRunExperienceTest.kt`
- Modify: `android/app/src/main/java/dev/waycontrol/MainActivity.kt`

**Interfaces:**
- `shouldShowFirstRunWelcome(hasSavedConnection: Boolean, welcomeCompleted: Boolean): Boolean` returns true only when no connection exists and onboarding has not been completed.
- Main activity resolves stored credentials before rendering Welcome or the existing dashboard.

- [x] Write a unit test for fresh install, persisted skip, and saved connection cases.
- [x] Run the focused test and confirm it fails because the route helper is missing.
- [x] Implement the route helper and persist `welcome_completed` in the existing app preferences.
- [x] Run focused JVM tests.

### Task 2: Welcome route and existing pairing controls

**Files:**
- Modify: `android/app/src/main/java/dev/waycontrol/MainActivity.kt`
- Test: `android/app/src/androidTest/java/dev/waycontrol/FirstRunExperienceUiTest.kt`

**Interfaces:**
- Render Welcome without bottom navigation while the first-run route is active.
- Reuse Settings endpoint/fingerprint/code inputs, QR scanner, LAN discovery and pairing callbacks.
- Skip persists completion and opens Home; an approved pair persists credentials, completes onboarding, and opens Home.

- [x] Add UI assertions for fresh Welcome, pairing controls, skip-to-Home, and Settings pairing after skip.
- [x] Run the Compose instrumentation test and confirm Welcome and transition assertions fail first.
- [x] Implement the welcome presentation around the existing pairing/discovery controls.
- [x] Run the focused UI test and full Android suite.

### Task 3: Regression verification and documentation

**Files:**
- Modify: `README.md`, `PROGRESS.md`

- [x] Run `./gradlew testDebugUnitTest lintDebug assembleDebug assembleRelease connectedDebugAndroidTest`.
- [x] Record device scope and results without claiming physical Poco M5 validation.
- [x] Run `git diff --check`, commit the verified checkpoint, and push it to `main`.
