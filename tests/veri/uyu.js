// Sahte torun: kendi PID'ini <dosya>'ya yazar ve sonsuza dek uyur.
// `node uyu.js <dosya> cocuk` → ayrıca kendi çocuğunu açar (<dosya>.arac): MCP'nin "araç" çocuğu benzeri.
const fs = require('fs');
const { spawn } = require('child_process');
const [, betik, dosya, tur] = process.argv;
if (tur === 'cocuk') {
  spawn(process.execPath, [betik, dosya + '.arac'], { stdio: 'ignore' });
}
fs.writeFileSync(dosya, String(process.pid));
setInterval(() => {}, 1e6);
