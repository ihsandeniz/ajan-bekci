// Sahte ajan (Windows): node ile çalışır, üç tür torun açar ve PID'lerini <onek>.* dosyalarına yazar.
//   <onek>.ajan  kendisi
//   <onek>.mcp   stdio borulu MCP benzeri sunucu (+ <onek>.mcp.arac: onun çocuğu)
//   <onek>.sb    `cmd /c start /b` ile kopan torun (cmd hemen çıkar → ebeveyni ölü)
//   <onek>.ayrik `detached: true` torun (kendi süreç grubu; Linux'taki setsid karşılığı)
// Kip: `cik <kod>` → torunlar yazılınca <kod> ile çıkar · `bekle` → sonsuza dek yaşar.
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const [, , onek, kip, kod] = process.argv;
const uyu = path.join(__dirname, 'uyu.js');
fs.writeFileSync(onek + '.ajan', String(process.pid));
spawn(process.execPath, [uyu, onek + '.mcp', 'cocuk'], { stdio: ['pipe', 'pipe', 'inherit'] });
// cmd satırı verbatim gider: yolları tırnakla (node `C:\Program Files\…` altında).
const t = (y) => '"' + y + '"';
spawn('cmd', ['/c', 'start', '/b', '""', t(process.execPath), t(uyu), t(onek + '.sb')], { stdio: 'ignore', windowsVerbatimArguments: true });
spawn(process.execPath, [uyu, onek + '.ayrik'], { stdio: 'ignore', detached: true }).unref();
const hepsi = ['.mcp', '.mcp.arac', '.sb', '.ayrik'].map((e) => onek + e);
const bekle = setInterval(() => {
  if (hepsi.every((d) => fs.existsSync(d)) && kip === 'cik') {
    clearInterval(bekle);
    process.exit(Number(kod || 0));
  }
}, 50);
