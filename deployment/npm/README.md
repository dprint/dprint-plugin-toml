# @dprint/toml

npm distribution of [dprint-plugin-toml](https://github.com/dprint/dprint-plugin-toml).

Use this with [@dprint/formatter](https://github.com/dprint/js-formatter) or just use @dprint/formatter and download the [dprint-plugin-toml WASM file](https://github.com/dprint/dprint-plugin-toml/releases).

## Example

```ts
import { createFromBuffer } from "@dprint/formatter";
import { getBuffer } from "@dprint/toml";

const formatter = createFromBuffer(getBuffer());

console.log(formatter.formatText("test.toml", "key   =   5"));
```
