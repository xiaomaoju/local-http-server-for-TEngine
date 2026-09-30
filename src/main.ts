import { createApp } from "vue";
import App from "./App.vue";
import "./styles/main.css";
import { installInputDefaults } from "../shared/inputDefaults";

const app = createApp(App);
app.mount("#app");
app.onUnmount(installInputDefaults());
import '../shared/theme.css';
