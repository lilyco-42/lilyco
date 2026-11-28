import tailwindcss from "@tailwindcss/vite";
import vue from "@vitejs/plugin-vue";
import { rapidVite } from "./vite.rapid";

export default rapidVite([vue(), tailwindcss()]);
