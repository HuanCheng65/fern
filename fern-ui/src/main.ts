import { mount } from "svelte";
import App from "./App.svelte";
import { host } from "fern-kit/host";
import { renderBiome, supportsBiomeWorker } from "./lib/biome-client";

// kit 里的封面自己不认识 Worker（官网那边就没有），把这台常驻的交给它。
if (supportsBiomeWorker) {
  host.paintOffscreen = (w, h, options, phase, quality) =>
    renderBiome(w, h, options, phase, quality).promise;
}

mount(App, { target: document.getElementById("app")! });
