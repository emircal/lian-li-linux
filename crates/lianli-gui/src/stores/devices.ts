import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { DeviceInfo, TelemetrySnapshot } from "../types";

export const useDeviceStore = defineStore("devices", () => {
  const devices = ref<DeviceInfo[]>([]);
  const telemetry = ref<TelemetrySnapshot>({
    fan_rpms: {},
    coolant_temps: {},
    streaming_active: false,
  });
  const daemonConnected = ref(false);
  const loading = ref(false);
  const error = ref<string | null>(null);

  // Callback invoked when daemon connection is first detected (or re-established)
  let onConnectCallback: (() => void) | null = null;
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  function onDaemonConnect(cb: () => void) {
    onConnectCallback = cb;
  }

  async function checkDaemon(): Promise<boolean> {
    try {
      const wasConnected = daemonConnected.value;
      const connected = await invoke<boolean>("connect_daemon");
      daemonConnected.value = connected;
      // Fire callback on first connection or reconnection
      if (connected && !wasConnected && onConnectCallback) {
        onConnectCallback();
      }
      return connected;
    } catch {
      daemonConnected.value = false;
      return false;
    }
  }

  async function refreshDevices(initial = false) {
    if (!daemonConnected.value) return;
    try {
      if (initial) loading.value = true;
      devices.value = await invoke<DeviceInfo[]>("list_devices");
      error.value = null;
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  }

  async function refreshTelemetry() {
    if (!daemonConnected.value) return;
    try {
      telemetry.value = await invoke<TelemetrySnapshot>("get_telemetry");
    } catch {
      // Silently ignore telemetry errors
    }
  }

  function startPolling(intervalMs = 2000) {
    stopPolling();
    pollTimer = setInterval(async () => {
      await checkDaemon();
      if (daemonConnected.value) {
        await refreshDevices();
        await refreshTelemetry();
      }
    }, intervalMs);
  }

  function stopPolling() {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }

  return {
    devices,
    telemetry,
    daemonConnected,
    loading,
    error,
    checkDaemon,
    refreshDevices,
    refreshTelemetry,
    startPolling,
    stopPolling,
    onDaemonConnect,
  };
});
