Punktfunk for TrimUI Smart Pro S
================================

1. Run "Punktfunk Setup" from Ports.
2. Select or enter your Punktfunk host.
3. Enter the PIN shown by the host/web console.
4. Launch "Punktfunk".

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
  cd /mnt/SDCARD/Ports/punktfunk   # path may vary by firmware
  source ./runtime-env.sh
  ./bin/punktfunk discover
  ./bin/punktfunk hosts list --probe
  ./bin/punktfunk-session --help
