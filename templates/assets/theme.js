(() => {
  'use strict';
  document.documentElement.lang = 'zh-CN';
  const words = {
    'New': '新建', 'List': '全部分享', 'Guide': '使用说明',
    'Expiration': '保存时间', 'Burn After': '阅后即焚', 'Syntax': '代码高亮', 'Privacy': '可见范围', 'Password': '访问密码',
    'Content': '分享内容', 'Save': '创建分享', 'None': '纯文本', 'Automatic': '自动识别',
    'Expire after': '保存时长', 'Burn after': '阅读次数', 'Client-Rendered': '浏览器高亮', 'Server-Rendered': '指定语言',
    'Unencrypted (no password)': '无需密码', 'Unencrypted (protected)': '密码保护',
    'Public': '公开', 'Unlisted': '仅链接可见', 'Read-only': '只读',
    'Never Expire': '永久保存', 'Never': '永久', 'No Limit': '不限次数', 'First Read': '阅读 1 次后删除',
    '10th Read': '阅读 10 次后删除', '100th Read': '阅读 100 次后删除', '1000th Read': '阅读 1000 次后删除', '10000th Read': '阅读 10000 次后删除',
    'Select or drop file attachment': '选择文件，或将文件拖到这里', 'Drop your file here': '松开鼠标即可添加文件',
    'Copy Text': '复制内容', 'Copy URL': '复制链接', 'Copy Redirect': '复制短链接', 'Copy': '复制', 'Copied': '已复制',
    'Raw Text Content': '原始文本', 'Edit': '编辑', 'Remove': '删除', 'Download': '下载', 'Download All': '下载全部',
    'Uploads': '文本与文件', 'URL Redirects': '短链接', 'Key': '分享编号', 'Created': '创建时间', 'Contents': '内容', 'Text': '文本',
    'QR Code': '分享二维码', 'QR': '二维码', 'Show QR': '显示二维码', 'Back to Upload': '← 返回分享',
    'Gallery': '网格', 'Stream': '纵向', 'Gallery View': '网格视图', 'Stream View': '纵向视图',
    'Uploading...': '上传中…', 'Processing...': '处理中…', 'Encrypting...': '加密中…', 'Zipping...': '打包中…', 'Failed': '上传失败，请重试',
    '1 minute': '1 分钟', '10 minutes': '10 分钟', '1 hour': '1 小时', '24 hours': '24 小时',
    '3 days': '3 天', '1 week': '1 周', '1 month': '1 个月', '6 months': '6 个月',
    '1 year': '1 年', '2 years': '2 年', '4 years': '4 年', '8 years': '8 年', '16 years': '16 年',
  };
  function translateText(text) {
    const key = text.trim().replace(/\s+/g, ' ');
    let value = words[key];
    if (!value && /^Attached: \d+ files$/.test(key)) value = key.replace(/^Attached: (\d+) files$/, '已选择 $1 个文件');
    if (!value && key.startsWith('Attached: ')) value = '已选择：' + key.slice(10);
    if (!value && key.startsWith('Drop your file here to replace ')) value = '拖入文件以替换：' + key.slice(31);
    if (!value && /^Please select a file smaller than \d+ MB$/.test(key)) value = key.replace(/^Please select a file smaller than (\d+) MB$/, '请选择小于 $1 MB 的文件');
    return value ? text.replace(text.trim(), value) : text;
  }
  // Translate interface nodes only; never visit uploaded content or file names.
  function translate(root) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: node => node.parentElement.closest('script, style, pre, code, textarea, .file-card, [download]') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
    });
    let node;
    while ((node = walker.nextNode())) {
      const value = translateText(node.nodeValue);
      if (value !== node.nodeValue) node.nodeValue = value;
    }
    if (root.matches('input[type="submit"]')) {
      const value = translateText(root.value);
      if (value !== root.value) root.value = value;
    }
    if (root.matches('optgroup')) root.label = translateText(root.label);
  }
  const nav = document.getElementById('nav');
  if (nav) {
    const brand = document.createElement('a');
    brand.href = nav.querySelector(':scope > a').href;
    const mark = document.createElement('span');
    mark.className = 'brand-mark'; mark.setAttribute('aria-hidden', 'true'); mark.textContent = '↗';
    brand.append(mark, document.title.split(' · ')[0]);
    nav.querySelector('b').replaceChildren(brand);
    nav.querySelectorAll(':scope > a').forEach(link => {
      if (new URL(link.href).pathname === location.pathname) link.setAttribute('aria-current', 'page');
    });
  }
  const footer = document.querySelector('.site-footer');
  if (footer) footer.classList.add('site-footer');
  const selector = '#nav, label, option, optgroup, button, input[type="submit"], th, .copy-button, #attach-file-button, #view-selector';
  document.querySelectorAll(selector).forEach(translate);
  const input = document.getElementById('content-input');
  if (input) {
    input.placeholder = '粘贴文字、代码或网址，也可以添加图片和文件…';
    const form = input.closest('form');
    const settings = document.getElementById('settings');
    const label = input.previousElementSibling;
    input.id && label.setAttribute('for', input.id);
    form.insertBefore(label, settings);
    form.insertBefore(input, settings);
    const intro = document.createElement('section');
    intro.className = 'intro';
    intro.innerHTML = '<span class="eyebrow">简单存，轻松传</span><h1>随手一放，即刻分享。</h1><p>文字、代码、图片与文件，一个链接就够了。粘贴网址即可生成短链接。</p>';
    form.before(intro);
    const privacy = document.getElementById('privacy');
    const password = document.getElementById('password_field');
    if (privacy && password) {
      const updatePassword = () => { password.parentElement.style.display = ['public', 'unlisted'].includes(privacy.value) ? 'none' : ''; };
      updatePassword(); privacy.addEventListener('change', updatePassword);
    }
    const attach = document.getElementById('attach-file-button');
    if (attach) {
      attach.setAttribute('tabindex', '0');
      attach.addEventListener('keydown', event => {
        if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); document.getElementById('file').click(); }
      });
    }
  }
  // These roots contain UI labels only, including asynchronous copy/upload feedback.
  document.querySelectorAll('button, input[type="submit"], .copy-button, #attach-file-button').forEach(root => {
    new MutationObserver(() => translate(root)).observe(root, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['value'] });
  });
  if (/^\/(upload|p)\//.test(location.pathname)) {
    document.querySelectorAll('body > div[style*="float:"]').forEach(root => {
      root.classList.add('share-toolbar'); translate(root);
    });
    document.querySelectorAll('a[href*="/qr/"]').forEach(a => { a.textContent = '分享二维码'; });
    document.querySelectorAll('a[download]').forEach(a => {
      if (a.id === 'download-link') a.textContent = '下载文件';
    });
    const stats = Array.from(document.querySelectorAll('body > div > p')).find(p => /^Read \d+ times?, last/.test(p.textContent.trim()));
    if (stats) stats.textContent = stats.textContent.replace(/Read (\d+) times?, last\s+/, '已浏览 $1 次 · 最近访问：').replace('just now', '刚刚').replace(/(\d+) seconds? ago/, '$1 秒前').replace(/(\d+) minutes? ago/, '$1 分钟前').replace(/(\d+) hours? ago/, '$1 小时前').replace(/(\d+) days? ago/, '$1 天前').replace(/(\d+)s ago/, '$1 秒前').replace(/(\d+)m ago/, '$1 分钟前').replace(/(\d+)h ago/, '$1 小时前').replace(/(\d+)d ago/, '$1 天前');
  }
  if (/^\/qr\//.test(location.pathname)) {
    document.querySelectorAll('body > div[style*="float:"]').forEach(translate);
    const card = document.querySelector('body > div[style*="text-align: center"]');
    if (card) {
      card.classList.add('qr-card');
      const heading = document.createElement('h2'); heading.textContent = '扫码，即刻打开'; card.prepend(heading);
      const hint = document.createElement('p'); hint.textContent = '用手机扫描二维码，查看这条分享。'; card.append(hint);
    }
  }
  if (location.pathname === '/list') {
    document.querySelectorAll('body > h3, th, td:not(:has(a)), td > a:not([href*="/upload/"]):not([href*="/file/"]):not([href*="/u/"])').forEach(translate);
    const empty = Array.from(document.querySelectorAll('body > p')).find(p => p.textContent.includes('No uploads yet.'));
    if (empty) empty.innerHTML = '还没有分享内容。<a href="/">创建第一个分享 →</a>';
  }
  if (/^\/edit\//.test(location.pathname)) {
    const title = document.querySelector('form h4');
    if (title) title.textContent = '编辑分享';
    const submit = document.getElementById('submit-button');
    if (submit) submit.value = '保存修改';
  }
})();
