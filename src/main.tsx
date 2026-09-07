import React from 'react'
import ReactDOM from 'react-dom/client'
import '@fontsource-variable/manrope'
import App from './App'
import { initializeDesktopBridge } from './tauriBridge'
import './styles.css'
import './activity-chart.css'

function render() {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode><App /></React.StrictMode>,
  )
}

void initializeDesktopBridge().finally(render)
