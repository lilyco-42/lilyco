import fs from 'node:fs'
const raw=JSON.parse(fs.readFileSync(new URL('../catalog.json',import.meta.url),'utf8'))
if(raw.schema_version!==1||!Array.isArray(raw.entries))throw Error('invalid schema')
const ids=new Set()
for(const p of raw.entries){
 if(!/^[a-z0-9-]{2,64}$/.test(p.id)||ids.has(p.id))throw Error('bad or duplicate id: '+p.id)
 ids.add(p.id)
 if(!['first_party','community'].includes(p.origin)||p.approved!==true)throw Error('unreviewed project: '+p.id)
 if(typeof p.description!=='string'||p.description.length>300)throw Error('bad description: '+p.id)
 const url=new URL(p.repository)
 if(url.protocol!=='https:'||url.hostname!=='github.com'||url.username||url.password||url.search||url.hash||url.pathname.split('/').filter(Boolean).length!==2){
  throw Error('repository must be a plain github.com owner/repo URL: '+p.id)
 }
 if(url.pathname.split('/')[1]!==p.publisher)throw Error('publisher mismatch: '+p.id)
 if(p.origin==='first_party'&&p.publisher!=='lilyco-42')throw Error('invalid owner: '+p.id)
}
console.log('Catalog valid:',raw.entries.length,'approved projects')
