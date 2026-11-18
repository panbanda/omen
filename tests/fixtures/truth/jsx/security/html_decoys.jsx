import { Panel } from "./helpers";

export function repositoryHtml(text) {
  const panel = new Panel();
  panel.insertAdjacentHTML("beforeend", text);
}
