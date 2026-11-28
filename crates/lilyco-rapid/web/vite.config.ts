import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { rapidVite } from "./vite.rapid";

export default rapidVite([react(), tailwindcss()]);
