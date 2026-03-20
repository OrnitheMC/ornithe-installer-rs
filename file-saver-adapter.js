import { saveAs } from "file-saver";

export function saveFile(buf, name) {
    saveAs(buf, name, { autoBom: true });
}