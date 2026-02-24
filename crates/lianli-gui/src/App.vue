<script setup lang="ts">
import Sidebar from "./components/Sidebar.vue";
import { useDeviceStore } from "./stores/devices";
import { useConfigStore } from "./stores/config";
import { onMounted, onUnmounted } from "vue";

const deviceStore = useDeviceStore();
const configStore = useConfigStore();

onMounted(async () => {
  // Load config whenever daemon connects (or reconnects)
  deviceStore.onDaemonConnect(async () => {
    await deviceStore.refreshDevices(true);
    await configStore.load();
  });

  await deviceStore.checkDaemon();
  if (deviceStore.daemonConnected) {
    await deviceStore.refreshDevices(true);
    await configStore.load();
  }
  deviceStore.startPolling(2000);
});

onUnmounted(() => {
  deviceStore.stopPolling();
});
</script>

<template>
  <div class="flex h-screen bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100">
    <Sidebar />
    <main class="flex-1 overflow-y-auto px-6 pb-6">
      <div
        v-if="!deviceStore.daemonConnected"
        class="mt-6 mb-4 rounded-lg bg-amber-50 dark:bg-amber-900/30 border border-amber-200 dark:border-amber-700 px-4 py-3 text-amber-800 dark:text-amber-200 text-sm"
      >
        Daemon not running. Start it with:
        <code class="ml-1 font-mono text-xs bg-amber-100 dark:bg-amber-800/50 px-1.5 py-0.5 rounded">
          sudo systemctl start lianli-daemon
        </code>
      </div>
      <router-view />
    </main>
  </div>
</template>
