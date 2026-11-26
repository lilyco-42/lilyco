import { createRoot } from "react-dom/client";
import { OfficeWorkspace } from "./office-workspace";
import "@fontsource-variable/noto-sans-sc";
import "./office-theme.css";

createRoot(document.getElementById("root")!).render(<OfficeWorkspace />);
