(() => {
  const select = document.getElementById('short-link-format');
  const custom = document.getElementById('short-link-custom');
  const hint = document.getElementById('link-format-hint');
  const form = select.form;
  const initial = select.dataset.default;
  if (Array.from(select.options).some(option => option.value === initial && initial !== 'custom')) select.value = initial;
  else { select.value = 'custom'; custom.value = initial; }
  let sequence = 0;
  let timer;
  async function update() {
    const request = ++sequence;
    custom.hidden = custom.disabled = select.value !== 'custom';
    custom.required = !custom.disabled;
    select.setCustomValidity('');
    hint.classList.remove('error-text');
    hint.textContent = '正在检查格式容量…';
    try {
      const response = await fetch(form.action + '/options/availability', {
        method: 'POST', headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({format: select.value, custom: custom.value}),
      });
      if (request !== sequence) return;
      if (!response.ok) throw new Error(await response.text());
      const data = await response.json();
      if (request !== sequence) return;
      if (!data.available) throw new Error(data.message);
      hint.textContent = `示例 /${data.example}` + (data.capacity ?
        ` · 剩余 ${BigInt(data.remaining).toLocaleString('zh-CN')} / ${BigInt(data.capacity).toLocaleString('zh-CN')} 个编号` : ' · 原版生成规则');
      if (select.value === 'custom') hint.textContent += ' · 支持固定前缀、[a-z]、[abc]、{5}；最多 32 位';
    } catch (error) {
      if (request !== sequence) return;
      const network = error instanceof TypeError;
      hint.textContent = network ? '暂时无法检查容量，提交时会再次检查。' : error.message;
      hint.classList.add('error-text');
      select.setCustomValidity(network ? '' : error.message);
    }
  }
  select.addEventListener('change', update);
  custom.addEventListener('input', () => {
    ++sequence;
    clearTimeout(timer);
    select.setCustomValidity('');
    timer = setTimeout(update, 250);
  });
  update();
})();
