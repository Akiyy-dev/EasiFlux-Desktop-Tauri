import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { tauriInvoke } from './composables/useTauriCommand'
import './assets/styles/global.css'

const app = createApp(App)
app.use(createPinia())
app.mount('#app')

if (import.meta.env.DEV) {
  ;(window as Window & { __easifluxInvoke?: typeof tauriInvoke }).__easifluxInvoke =
    tauriInvoke
}
