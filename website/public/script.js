const previewOpen = document.querySelector('#preview-open');
const previewClose = document.querySelector('#preview-close');
const previewDialog = document.querySelector('#preview-dialog');

if (typeof previewDialog.showModal === 'function') {
  previewOpen.hidden = false;
  previewOpen.addEventListener('click', () => previewDialog.showModal());
  previewClose.addEventListener('click', () => previewDialog.close());
  previewDialog.addEventListener('click', (event) => {
    if (event.target !== previewDialog) return;
    const bounds = previewDialog.getBoundingClientRect();
    if (event.clientX < bounds.left || event.clientX > bounds.right ||
        event.clientY < bounds.top || event.clientY > bounds.bottom) {
      previewDialog.close();
    }
  });
}
