import { createRoot } from 'react-dom/client';
import '@fontsource-variable/noto-sans-sc';
import './media-theme.css';
import { MediaWorkspace } from './media-workspace';

createRoot(document.getElementById('root')!).render(<MediaWorkspace/>);
