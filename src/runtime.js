const { core } = Deno;
const { ops } = core;

async function* readStdinLines() {
  while (true) {
    const chunk = await core.opAsync("op_read_stdin_next"); // Call the Rust op
    if (chunk === null) {
      // If the chunk is null, we've reached EOF and can break the loop
      break;
    }
    for (const line of chunk) {
      yield line;
    }
  }
}

function argsToMessage(...args) {
  return args.map((arg) =>
    typeof arg === "string" ? arg : JSON.stringify(arg)
  );
}

const console = {
  log: (...args) => {
    core.print(`${argsToMessage(...args)}\n`, false);
  },
  error: (...args) => {
    core.print(`[err]: ${argsToMessage(...args)}\n`, true);
  },
};

globalThis.setTimeout = (callback, delay) => {
  core.opAsync("op_set_timeout", delay).then(callback);
};
globalThis.console = console;
globalThis.stdin = readStdinLines();
