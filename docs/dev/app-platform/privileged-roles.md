<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Privileged App Roles & Access Portals (roadmap)

Reference + roadmap for the **special app classes** that need privileges a normal
sandboxed app must not have, and the **portal/picker** mechanism that lets normal
apps touch sensitive data *without* those privileges. Cross-platform survey
(Android / iOS / OHOS) mapped onto our model, so the set of `bundle_type`s and
`nexus.permission.*` grows deliberately, not ad hoc.

> This is a planning doc. "Built" = shipped; "planned" links the owning task.

## Our model (recap)

Two orthogonal axes, plus one escape valve:

- **`bundle_type` = privilege ceiling** — what a bundle MAY request, enforced
  fail-closed at pack time (`nxb-pack`). A normal `app` cannot even ship a
  system-role permission. (Parallels OHOS's APL tiers `normal` /
  `system_basic` / `system_core`.)
- **`product.toml` = role assignment** — WHICH bundle plays a system role
  (`shell = <id>`, `greeter = <id>`). Any app is assignable; autologin
  (`session = auto`) needs no greeter at all.
- **Launcher-visibility** is a SEPARATE axis from privilege: `app` + `settings`
  (+ future launchable-privileged types) show in the grid; `shell`/`greeter`
  and non-UI types do not.
- **Portals** — the mediated, user-consented path (`docs`: mediated-then-direct
  via abilitymgr): a normal app declares an intent, the OS shows a picker, and
  the app gets a **scoped, revocable handle**, never a broad permission.

## A. System surfaces — assigned, not user-launched

The OS/product picks these; they never appear in the launcher grid.

| Role | Android | iOS | OHOS | Our type / grant | Status |
|---|---|---|---|---|---|
| Home / shell / launcher | `ROLE_HOME` | SpringBoard | Launcher (`system_core`) | `bundle_type = shell` (`LAUNCH`/`ENUMERATE`) | **Built** |
| Greeter / lock / keyguard | Keyguard | Lock screen | Screenlock | `bundle_type = greeter` (`SESSION`) | **Built** (avatar built-in; DSL swap planned) |
| Status bar / quick settings / notif shade | SystemUI | Control Center | SystemUI | systemui surfaces (not app bundles) | Partial (TASK-0119+) |
| Volume / media / screenshot / screen record | SystemUI | SystemUI | SystemUI | systemui surfaces | Planned |
| Input method / keyboard (IME) | `BIND_INPUT_METHOD` | Keyboard ext | IME kit | role `ime` (sees all input — high trust) | Planned (TASK-0147/0150) |
| Wallpaper / live wallpaper | `WallpaperService` | — | Wallpaper | systemui surface | Partial |
| Setup wizard / provisioning | Setup Wizard | Setup | — | first-boot role | Planned |

## B. Privileged **launchable** apps — in the grid, elevated caps

User opens these; they carry a power a normal app must not.

| App | Android | iOS | OHOS | Our type / permission | Status |
|---|---|---|---|---|---|
| Settings | Settings (privileged) | Settings | Settings | `bundle_type = settings` (`SETTINGS`) | **Built** |
| Files / documents | Files / DocumentsUI | Files | FileManager | `bundle_type = filemanager` (`FILES`) | Planned (TASK-0083/0084) |
| Phone / dialer | `ROLE_DIALER` | Phone | Phone/Call | role `dialer` (`CALL`) | Planned |
| Messaging / SMS | `ROLE_SMS` | Messages | MMS | role `sms` (`SMS_STORE`) | Planned |
| Contacts | Contacts + provider | Contacts | Contacts | role `contacts` + provider; others via picker | Planned |
| Camera | default camera | Camera | Camera | `CAMERA` (any app) + a camera app | Planned (TASK-0106) |
| Gallery / photos | Photos + MediaProvider | Photos | Gallery | provider + app; others via photo-picker | Planned (TASK-0090) |
| Browser | `ROLE_BROWSER` | default browser | Browser | role `browser` (http/https handler) | Planned (TASK-0113) |
| Email | `ROLE_EMAIL` | Mail | Email | role `email` (mailto handler) | Planned |
| Wallet / payments | `ROLE_WALLET`, HCE | Wallet | Wallet | role `wallet` (`NFC_PAY`) | Later |
| Clock / alarm | AlarmClock | Clock | Clock | `ALARM` (any app) | Later |
| App store / installer | Package Installer | App Store | AppGallery | `bundle_type` install-privileged (`INSTALL`) | Planned (TASK-0131) |

## C. Privileged **background** services / default handlers

No launcher entry of their own; user selects them as "default X" and grants a
strong capability. These are `bundle_type = service` bundles with a gated permission.

| Role | Android | iOS | OHOS | Our grant | Status |
|---|---|---|---|---|---|
| Autofill / password manager / credentials | `AutofillService`, CredentialManager | AutoFill, Passwords | — | `CREDENTIALS` | Later |
| Notification listener / relay | `BIND_NOTIFICATION_LISTENER` | — | — | `NOTIFY_LISTEN` | Later |
| Accessibility service | `AccessibilityService` | — | Accessibility | `A11Y_CONTROL` (very high trust) | Planned (TASK-0118) |
| VPN | `VpnService` | Network ext | VPN | `VPN` | Later |
| Print | `PrintService` | AirPrint | Print | `PRINT` | Partial (TASK-0089/0090 print) |
| Backup / restore | BackupAgent | — | Backup | `BACKUP` | Later |
| Device admin / MDM | DevicePolicyManager | MDM | EDM | `DEVICE_ADMIN` | Later |
| Assistant / voice | `ROLE_ASSISTANT` | Siri | AI assistant | role `assistant` | Later |

## D. Access portals — how NORMAL (sandboxed) apps touch sensitive data

The core sandbox principle: a normal app gets **no** broad permission; it asks
through a portal, the user picks, and the app receives a **scoped, revocable
handle** to exactly what was chosen. This is our *mediated-then-direct* pattern
(abilitymgr resolves + mints the channel) applied to data access.

| Portal | Android | iOS | OHOS | Our mechanism | Status |
|---|---|---|---|---|---|
| Document picker (open/save/open-with) | SAF `ACTION_OPEN_DOCUMENT` | UIDocumentPicker | DocumentViewPicker | scoped-URI grant | Planned (TASK-0083 + 0084) |
| Photo picker | Photo Picker | PHPicker | PhotoViewPicker | scoped media handle | Planned |
| Contact picker | Contacts picker | CNContactPicker | ContactPicker | scoped contact handle | Planned |
| Share sheet | Share intent | Share sheet | Share | one-shot export target | Planned |
| Camera capture (get one photo) | `ACTION_IMAGE_CAPTURE` | UIImagePicker | — | camera app → scoped result | Planned (TASK-0106) |
| One-shot location | one-time location | one-time location | — | scoped, single-fix grant | Later |
| One-tap grant (security component) | — | paste consent | Security UI components | user-tap = one-shot cap | Later |

**Why portals over permissions:** a file manager is the ONE app with broad
`FILES`; every other app that "opens a file" uses the document picker and only
ever sees the picked file (`TASK-0084` scoped-URI grants). Same for photos and
contacts. The privilege stays with the one purpose-built app; everyone else is
sandboxed + user-consented.

## Design guidance — which mechanism for a new capability?

1. **Distinct `bundle_type`** — when it is a user-recognisable *system app*
   with a durable identity + special powers (settings, files, phone). The type
   names the app and can later carry default-handler / "open with" behaviour.
2. **Portal / picker** — when a NORMAL app needs occasional, user-chosen access
   to sensitive data (files, photos, contacts). Prefer this over a broad cap.
3. **Plain gated permission on `service`/`app`** — for a background capability
   without its own app identity (print, vpn), user-selected as a default.
4. **Never** a broad ambient permission a normal `app` can just declare — the
   pack-time ceiling must reject it.

## Related

- `docs/dev/app-platform/` — this dir (app anatomy, roles, packaging).
- `tools/nexus-idl/schemas/manifest.capnp` — `BundleType` (the ceiling enum).
- `source/services/abilitymgr/src/caps.rs` — `KNOWN_PERMISSIONS` (grows here).
- TASK-0083 document-picker · TASK-0084 scoped-URI grants · TASK-0106 camera ·
  TASK-0113 browser · TASK-0118 accessibility · TASK-0131 installer.
