import { statSync } from 'node:fs'
import { resolve } from 'node:path'
const filename = resolve('frontend/dist/index.html')
const size = statSync(filename).size
const max = 1024 * 1024
if (size > max) throw Error('Client bundle exceeded 1 MiB: ' + size + ' bytes. Move optional ML runtime behind an on-demand CDN loader.')
console.log('Single-file client bundle:', size, 'bytes; 1 MiB gate passed.')
