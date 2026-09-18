import { createRoot } from 'react-dom/client';
import './index.css';
import App from './App';

const root = document.getElementById('root');
if (!root) {
  throw new Error('找不到 #root 挂载节点');
}
createRoot(root).render(<App />);
