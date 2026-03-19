import { saveAs } from "file-saver";

export function saveFile(buf, name) {
    console.log("Saving file: " + name);
    saveAs(buf, name, { autoBom: true });
}