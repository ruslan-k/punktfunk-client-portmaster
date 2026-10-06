Punktfunk for TrimUI Smart Pro S
================================

1. Run "Punktfunk" from Ports to open the graphical gamepad host console.
2. Select or enter your Punktfunk host.
3. Enter the PIN shown by the host/web console using the on-screen controls.
4. Open the host library. A saved default host opens its library on launch.

There is only one launcher. Remove the obsolete "Punktfunk Setup.sh" when
updating an older installation.

No terminal or dialog package is required. On Spruce without a desktop display,
SDL3 selects KMSDRM and uses the firmware's VK_KHR_display Vulkan presentation.
The bundled SDL3 must include KMSDRM; physical UI validation is still required.

By default the port opens the controller-driven game library. Edit config.env
and set PUNKTFUNK_START_MODE=desktop to connect directly to the desktop.

Logs:
  punktfunk/logs/

Persistent trust/settings:
  punktfunk/state/

Important:
  Upstream punktfunk-session currently presents through SDL3 + Vulkan. This
  package includes an embedded Mali loader fallback, but the final WSI path is
  firmware/driver dependent and must be validated on the real TSPS.

Useful SSH diagnostics:
  cd /mnt/SDCARD/Roms/PORTS/punktfunk   # path may vary by firmware
  source ./runtime-env.sh
  ./bin/punktfunk discover
  ./bin/punktfunk hosts list --probe
  ./bin/punktfunk-session --help
