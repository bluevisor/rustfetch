// Reads the GPU and the main screen from app_server for src/platform/haiku.rs. BScreen needs a
// BApplication to reach app_server, which is why this is C++ rather than Rust

#include <Application.h>
#include <Screen.h>

#include <string.h>

extern "C" int
rustfetch_haiku_graphics(char* gpu, size_t gpuSize, char* screenName, size_t screenNameSize,
	uint32* screen)
{
	status_t status;
	BApplication app("application/x-vnd.rustfetch", &status);
	if (status != B_OK)
		return 0;

	int found = 0;
	BScreen mainScreen(B_MAIN_SCREEN_ID);
	if (!mainScreen.IsValid())
		return 0;

	// The chipset is what the accelerant knows the card as, its name is the driver's
	accelerant_device_info device;
	if (mainScreen.GetDeviceInfo(&device) == B_OK) {
		strlcpy(gpu, device.chipset[0] != '\0' ? device.chipset : device.name, gpuSize);
		found |= 1;
	}

	display_mode mode;
	if (mainScreen.GetMode(&mode) == B_OK) {
		screen[0] = mode.virtual_width;
		screen[1] = mode.virtual_height;
		// pixel_clock is in kHz, so this is the refresh rate in mHz
		uint64 pixels = (uint64)mode.timing.h_total * mode.timing.v_total;
		screen[2] = pixels > 0 ? (uint32)((uint64)mode.timing.pixel_clock * 1000000 / pixels) : 0;
		found |= 2;

		monitor_info monitor;
		if (mainScreen.GetMonitorInfo(&monitor) == B_OK)
			strlcpy(screenName, monitor.name, screenNameSize);
	}

	return found;
}
