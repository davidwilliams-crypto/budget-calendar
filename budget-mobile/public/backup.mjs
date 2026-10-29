import {sealSnapshot,openSnapshot} from './crypto.mjs';
export async function makeBackup(localState,key,budgetId,createdAt=new Date().toISOString()){
 // Encrypt the working snapshot exactly once. Pending queues may contain hundreds
 // of older snapshots; their replay receipts are unnecessary for snapshot recovery.
 // The informational metadata is untrusted and never used to replay a write.
 const revision=localState.revision,payload=await sealSnapshot(localState.working,key,budgetId,revision);
 const sync={pendingCount:localState.pending.length,hasConflict:Boolean(localState.conflict),conflictRevision:localState.conflict?.revision??null};
 return {format:'budget-calendar-encrypted-backup',version:2,budgetId,revision,createdAt,sync,payload};
}
export async function readBackup(saved,key,budgetId){
 if(saved?.format!=='budget-calendar-encrypted-backup'||![1,2].includes(saved.version)||saved.budgetId!==budgetId||!Number.isSafeInteger(saved.revision)||saved.revision<1)throw Error('Choose an encrypted backup for this household.');
 const decoded=await openSnapshot(saved.payload,key,budgetId,saved.revision);
 const snapshot=saved.version===2?decoded:decoded.snapshot;
 if(!snapshot||snapshot.id!==budgetId)throw Error('The snapshot belongs to a different household.');
 return {snapshot,sync:saved.version===2?saved.sync:{pendingCount:decoded.localState?.pending?.length??0,hasConflict:Boolean(decoded.localState?.conflict)}};
}
