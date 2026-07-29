const dateFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});

const numberFormatter = new Intl.NumberFormat();

export const formatDate = (timestamp: number | null) =>
  timestamp === null ? 'Unknown' : dateFormatter.format(new Date(timestamp));

export const formatBytes = (size: number) => {
  if (size < 1024) {
    return `${numberFormatter.format(size)} B`;
  }
  const units = ['KiB', 'MiB', 'GiB', 'TiB'];
  let value = size / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)} ${units[unitIndex]}`;
};
