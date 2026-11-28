import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { rapidVite } from "./vite.rapid";

export default rapidVite([svelte(), tailwindcss()]);
