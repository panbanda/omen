export function unsafeHtmlOutput(element, text) {
  element.innerHTML = text;
  element.textContent = text;
  const value = "element.innerHTML = text";
  // element.innerHTML = text;
  element.insertAdjacentHTML("beforeend", text);
  element.innerHTML = "<br>";
  return value;
}
