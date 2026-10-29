// Disabling controls preserves their values while excluding them from FormData.
// Occurrence commands accept only amount, note and status; series settings must
// remain unavailable until the user deliberately chooses a series scope.
export function bindEditorScope(form){
 const scope=form.elements.namedItem('scope');if(!scope)return;
 const seriesFields=['description','category','date','repeat','frequency','freq_type','end_mode','end_date','n_pays','loan_amount','grace_period_days','disabled','bit-1','bit-2','bit-4','bit-8','bit-256'];
 const update=()=>{const occurrence=scope.value==='one';for(const name of seriesFields){const field=form.elements.namedItem(name);if(field)field.disabled=occurrence}const hint=form.querySelector('[data-scope-hint]');if(hint)hint.textContent=occurrence?'Only amount, note and paid status are editable for this occurrence. Choose a series scope to change description, category, date or recurrence.':'Series settings are editable. Future changes split the payment at this occurrence; whole-series changes apply to every occurrence.'};
 scope.addEventListener('change',update);update();
}
