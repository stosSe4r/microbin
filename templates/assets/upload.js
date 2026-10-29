// Native File.slice + XHR: bounded requests and progress, without an upload library.
async function uploadShare(form, button) {
  const endpoint = form.action;
  const chunkBase = endpoint + '/chunks';
  const chunkSize = 16 * 1024 * 1024;
  const data = new FormData(form);
  const files = data.getAll('file').filter(file => file instanceof File && file.name);
  const total = files.reduce((sum, file) => sum + file.size, 0);
  const status = document.getElementById('upload-status');
  const progress = document.getElementById('upload-progress');
  const cancel = document.getElementById('upload-cancel');
  const controller = new AbortController();
  const tokens = [];
  const started = performance.now();
  const format = bytes => `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
  let phase = 'upload';
  status.hidden = false;
  progress.hidden = false;
  progress.value = 0;
  cancel.hidden = false;
  cancel.onclick = () => controller.abort();
  const onLeave = event => { event.preventDefault(); event.returnValue = ''; };
  window.addEventListener('beforeunload', onLeave);

  function update(loaded, size, label = '上传中') {
    const percent = size ? Math.min(100, Math.floor(loaded / size * 100)) : 0;
    progress.value = percent;
    button.value = percent === 100 ? '处理中…' : `上传 ${percent}%`;
    const speed = loaded / Math.max(1, (performance.now() - started) / 1000);
    status.textContent = `${label} · ${format(loaded)} / ${format(size)} · ${format(speed)}/s`;
  }

  function send(method, url, body, onProgress = () => {}, json = false) {
    return new Promise((resolve, reject) => {
      if (controller.signal.aborted) return reject(new DOMException('已取消上传', 'AbortError'));
      const xhr = new XMLHttpRequest();
      xhr.open(method, url);
      xhr.timeout = 5 * 60 * 1000;
      if (json) xhr.setRequestHeader('Content-Type', 'application/json');
      const abort = () => xhr.abort();
      controller.signal.addEventListener('abort', abort, {once: true});
      xhr.upload.onprogress = event => { if (event.lengthComputable) onProgress(event.loaded, event.total); };
      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) resolve(xhr);
        else {
          const message = xhr.status === 413 ? '请求超过 CDN 或站点的上传大小限制' :
            (xhr.responseText.length < 200 && !xhr.responseText.includes('<') ? xhr.responseText : `上传失败（HTTP ${xhr.status}）`);
          reject(Object.assign(new Error(message), {status: xhr.status}));
        }
      };
      xhr.onerror = () => reject(new Error('网络连接失败，请重试'));
      xhr.ontimeout = () => reject(new Error('上传超时，请重试'));
      xhr.onabort = () => reject(new DOMException('已取消上传', 'AbortError'));
      xhr.onloadend = () => controller.signal.removeEventListener('abort', abort);
      xhr.send(body);
    });
  }

  try {
    // Split the complete selection: many small files can also exceed the CDN limit.
    if (total > 64 * 1024 * 1024) {
      data.delete('file');
      let completed = 0;
      for (const file of files) {
        const init = await send('POST', chunkBase + '/start', JSON.stringify({name: file.name, size: file.size}), undefined, true);
        const {id} = JSON.parse(init.responseText);
        tokens.push(id);
        for (let offset = 0; offset < file.size; offset += chunkSize) {
          const part = file.slice(offset, offset + chunkSize);
          for (let attempt = 0; ; attempt++) {
            try {
              const response = await send('PUT', `${chunkBase}/${id}/${offset}`, part,
                loaded => update(completed + offset + loaded, total, `正在上传：${file.name}`));
              if (JSON.parse(response.responseText).received !== offset + part.size) throw new Error('分片确认失败，请重试');
              break;
            } catch (error) {
              if (error.name === 'AbortError' || (error.status >= 400 && error.status < 500) || attempt === 2) throw error;
              status.textContent = `连接中断，正在重试当前分片（${attempt + 1}/2）…`;
              await new Promise(resolve => setTimeout(resolve, (attempt + 1) * 1000));
            }
          }
        }
        completed += file.size;
        data.append('file_token', id);
      }
      update(total, total, '文件上传完成，正在创建分享');
    }
    // Do not retry the final POST: a lost response may already have created a share.
    phase = 'finalize';
    const response = await send('POST', endpoint, data, (loaded, size) => {
      if (!tokens.length) update(loaded, size);
    });
    const result = new URL(response.responseURL);
    if (!/\/(upload\/[^/]+|auth\/[^/]+\/success)$/.test(result.pathname)) {
      throw new Error('分享未创建成功，请检查上传密码后重试');
    }
    return result.href;
  } catch (error) {
    status.textContent = error.name === 'AbortError' ? (phase === 'finalize' ? '已停止等待结果，请先到「全部分享」确认是否已创建。' : '已取消上传，可重新选择文件或重试。') :
      `${error.message}${phase === 'finalize' && !error.status ? '；若文件已传完，请先到「全部分享」确认是否已创建。' : ''}`;
    throw error;
  } finally {
    cancel.hidden = true;
    progress.hidden = true;
    window.removeEventListener('beforeunload', onLeave);
    // DELETE is idempotent, including tokens consumed by the normal create endpoint.
    for (const id of tokens) fetch(`${chunkBase}/${id}`, {method: 'DELETE', keepalive: true}).catch(() => {});
  }
}
