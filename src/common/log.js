// Journalisation minimale avec horodatage.

const stamp = () => new Date().toISOString().replace('T', ' ').slice(0, 19);

export const log = {
  trace: process.env.ARCHCORE_TRACE === '1',
  debug: (m) => { if (log.trace) console.log(`${stamp()} DEBUG ${m}`); },
  info: (m) => console.log(`${stamp()} INFO  ${m}`),
  warn: (m) => console.warn(`${stamp()} WARN  ${m}`),
  error: (m) => console.error(`${stamp()} ERROR ${m}`),
};
