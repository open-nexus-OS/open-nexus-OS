---
name: Setup report (distro / host)
about: Report how `make initial-setup`, `make doctor` and `just start` went on your machine
title: "Setup report: <distro> <version>"
labels: testing
---

**Host**
- Distro and version (`cat /etc/os-release | head -3`):
- Kernel (`uname -r`):
- GPU and driver (`glxinfo -B | head -8`, if installed):
- Fresh install or daily machine:

**Results** (✅ / ❌ / skipped)
- [ ] `make initial-setup`
- [ ] `make doctor`
- [ ] `make build && make run` (headless boot)
- [ ] `just start` (desktop window appears, mouse and keyboard work)
- [ ] `just test-host`

**What went wrong or was confusing**
Paste the failing command and the last ~30 lines of its output. If `make doctor`
printed a fix command, say whether it worked.

**Time taken** from `git clone` to the desktop:
