import { createRoot } from 'react-dom/client'
import { App } from './App'
import { start } from './lib/app'
import './app.css'

createRoot(document.getElementById('root')!).render(<App />)
start()
