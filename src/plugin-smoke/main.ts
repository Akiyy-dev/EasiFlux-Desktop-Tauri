import { createApp } from 'vue'
import { createPinia } from 'pinia'
import PluginSmokeApp from './PluginSmokeApp.vue'
import '../assets/styles/global.css'

createApp(PluginSmokeApp).use(createPinia()).mount('#app')
