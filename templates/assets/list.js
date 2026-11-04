(() => {
  const toolbar = document.getElementById('batch-toolbar');
  const boxes = () => Array.from(document.querySelectorAll('.share-select'));
  const all = document.getElementById('select-all');
  const range = document.getElementById('select-range');
  const clear = document.getElementById('clear-selection');
  const copy = document.getElementById('copy-selected');
  const remove = document.getElementById('delete-selected');
  const status = document.getElementById('batch-status');
  let anchor = null;
  let rangeMode = false;
  let busy = false;
  const selected = () => boxes().filter(box => box.checked);
  function update() {
    const rows = boxes();
    const chosen = selected();
    all.checked = rows.length > 0 && chosen.length === rows.length;
    all.indeterminate = chosen.length > 0 && chosen.length < rows.length;
    all.disabled = busy || !rows.length;
    clear.disabled = copy.disabled = busy || !chosen.length;
    remove.disabled = busy || !chosen.length || chosen.some(box => box.dataset.deletable !== 'true');
    range.disabled = busy || !rows.length;
    document.getElementById('selection-count').textContent = `已选 ${chosen.length} / ${rows.length} 条`;
    document.getElementById('empty-list').hidden = rows.length > 0;
    document.querySelector('.share-list').hidden = rows.length === 0;
    rows.forEach(box => { box.disabled = busy; box.closest('tr').classList.toggle('is-selected', box.checked); });
    if (chosen.some(box => box.dataset.deletable !== 'true')) status.textContent = '包含受保护分享，可复制链接；删除请使用单条管理并验证密码。';
  }
  function resetRange() {
    rangeMode = false;
    range.setAttribute('aria-pressed', 'false');
  }
  all.addEventListener('change', () => { boxes().forEach(box => { box.checked = all.checked; }); anchor = null; resetRange(); status.textContent = ''; update(); });
  clear.addEventListener('click', () => { boxes().forEach(box => { box.checked = false; }); anchor = null; resetRange(); status.textContent = ''; update(); });
  range.addEventListener('click', () => {
    rangeMode = !rangeMode;
    range.setAttribute('aria-pressed', String(rangeMode));
    status.textContent = rangeMode ? (anchor ? '再勾选一个终点，将连续选择两点之间的分享。' : '先勾选起点，再勾选终点。') : '';
  });
  boxes().forEach(box => box.addEventListener('click', event => {
    const rows = boxes();
    if ((event.shiftKey || rangeMode) && anchor && rows.includes(anchor)) {
      const ends = [rows.indexOf(anchor), rows.indexOf(box)].sort((a, b) => a - b);
      rows.slice(ends[0], ends[1] + 1).forEach(row => { row.checked = box.checked; });
      resetRange();
    }
    anchor = box;
    status.textContent = rangeMode ? '再勾选一个终点。' : '';
    update();
  }));
  document.addEventListener('keydown', event => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a' && !event.target.closest('input:not([type="checkbox"]), textarea, [contenteditable="true"]')) {
      if (busy) return;
      event.preventDefault(); all.checked = true; all.dispatchEvent(new Event('change'));
    }
  });
  copy.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(selected().map(box => new URL(box.dataset.url, location.href).href).join('\n'));
      status.textContent = `已复制 ${selected().length} 条分享链接。`;
    } catch (_) { status.textContent = '复制失败，请在 HTTPS 页面中重试并允许剪贴板访问。'; }
  });
  remove.addEventListener('click', async () => {
    const chosen = selected();
    if (!chosen.length || !confirm(`确定删除这 ${chosen.length} 条分享及其文件？`)) return;
    busy = true; update();
    status.textContent = '正在删除…';
    try {
      const response = await fetch(toolbar.dataset.removeUrl, {method: 'POST', headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({slugs: chosen.map(box => box.value)})});
      if (!response.ok) throw new Error(await response.text());
      chosen.forEach(box => box.closest('tr').remove());
      status.textContent = `已删除 ${chosen.length} 条分享。`;
      anchor = null; resetRange();
    } catch (error) { status.textContent = `删除未完成：${error.message}。可刷新列表确认后重试。`; }
    finally { busy = false; update(); }
  });
  update();
})();
