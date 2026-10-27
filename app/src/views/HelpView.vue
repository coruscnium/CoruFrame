<script setup lang="ts">
import Icon from "../components/Icon.vue";
import PageHeader from "../components/PageHeader.vue";

const version = __APP_VERSION__;
</script>

<template>
  <div class="view">
    <div class="content">
      <PageHeader
        title="CoruFrame"
        description="Linux-native Warframe companion."
        svg-icon="help"
        rail-toggle
      >
        <template #actions>
          <span class="preview-chip"><Icon name="shield" :size="13" /> GPL-3.0</span>
          <span class="preview-chip"><Icon name="terminal" :size="13" /> v{{ version }}</span>
        </template>
      </PageHeader>

      <div class="about-hero">
        <div class="about-mark"><span>C</span></div>
        <div>
          <h2>Local by default.</h2>
          <p>Reads EE.log, public APIs and an optional read-only inventory snapshot.</p>
        </div>
      </div>

      <div class="about-grid">
        <section class="panel">
          <div class="panel-head"><h2>Reads</h2><Icon name="database" :size="16" /></div>
          <ul class="list">
            <li>EE.log</li>
            <li>Market and worldstate APIs</li>
            <li>Reward screen captures</li>
            <li>Optional inventory snapshot</li>
          </ul>
        </section>
        <section class="panel">
          <div class="panel-head"><h2>Never does</h2><Icon name="shield" :size="16" /></div>
          <ul class="list">
            <li>No injection or input synthesis</li>
            <li>No game-file writes or packet capture</li>
            <li>No telemetry or hosted account</li>
          </ul>
        </section>
        <section class="panel">
          <div class="panel-head"><h2>Privacy</h2><Icon name="lock" :size="16" /></div>
          <ul class="list">
            <li>Local data only</li>
            <li>Wallet-backed market token</li>
            <li>Owner-only files</li>
          </ul>
        </section>
        <section class="panel">
          <div class="panel-head"><h2>Disclaimer</h2><Icon name="help" :size="16" /></div>
          <p class="panel-copy">Unofficial. Use at your own risk.</p>
        </section>
      </div>

      <div class="panel">
        <div class="panel-head"><h2>FAQ</h2></div>
        <div class="faq-list">
          <details>
            <summary>Inventory not linked?</summary>
            <p>Enable the snapshot and launch Warframe through the wrapper.</p>
          </details>
          <details>
            <summary>Inventory stale?</summary>
            <p>Open the in-game inventory or change relay to trigger a sync.</p>
          </details>
          <details>
            <summary>How does the HUD read cards?</summary>
            <p>KWin capture and local Tesseract OCR.</p>
          </details>
          <details>
            <summary>Need an account?</summary>
            <p>No. Orders and presence need one.</p>
          </details>
          <details>
            <summary>Is the memory read safe?</summary>
            <p>Read-only and off by default. It is still a grey area.</p>
          </details>
        </div>
      </div>
    </div>
  </div>
</template>
