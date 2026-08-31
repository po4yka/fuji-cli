# Windows Driver Setup

Windows normally binds a camera to its WPD or photo-import driver, which blocks
raw PTP access. Replace that device driver with WinUSB or libusbK using Zadig.

1. Install Zadig from <https://zadig.akeo.ie/>.
2. Connect the camera in PTP or USB mode.
3. In Zadig, choose **Options -> List All Devices**.
4. Select the camera, often named **USB PTP** or shown by model name.
5. Select **WinUSB** (recommended) or **libusbK** as the target driver.
6. Choose **Replace Driver**.

The change applies to the selected Windows device. Zadig can restore another
driver later.

Verify the result with `fujicli device list`, then continue with the
[first safe session](../getting-started/first-safe-session.md). For failures,
use [device-access troubleshooting](troubleshoot-device-access.md).
