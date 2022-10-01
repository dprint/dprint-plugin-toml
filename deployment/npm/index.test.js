// @ts-check
const assert = require("assert");
const createFromBuffer = require("@dprint/formatter").createFromBuffer;
const getPath = require("./index").getPath;

const buffer = require("fs").readFileSync(getPath());
const formatter = createFromBuffer(buffer);
const result = formatter.formatText("file.toml", "key   =   5");

assert.strictEqual(result, "key = 5\n");
