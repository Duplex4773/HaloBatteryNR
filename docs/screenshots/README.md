# Native dashboard screenshots

Captured on 2 October 2026 from the Windows release build. Devices, battery
history, charge cycles and polling evidence are **synthetic simulation data**.
These images demonstrate the native UI; they establish no hardware verification.

From the repository root, close the app and run:

```powershell
.\tools\build-rust.ps1
.\tools\capture-docs.ps1 -Themes Dark,Light
```

The capture tool isolates all data under ignored `validation-local`, sends window
messages only to its simulation process, and captures its window rather than the
desktop. Explicit theme capture temporarily changes the Windows app-theme
preference, notifies only the simulation, and restores the preference on exit.
Use the default `-Themes Current` to leave that preference untouched.
