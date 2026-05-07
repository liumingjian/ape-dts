# 0007 — Brand: "ape-dts Console"

The prototype shipped under the name "DRS · Data Replication Service", which collides with Huawei Cloud's DRS product (the prototype already carries a disclaimer). We rebrand the management plane to **ape-dts Console**, matching the open-source engine name. Touchpoints: `BrandMark.vue`, login hero, footer copyright, `index.html` `<title>`, `web-prototype/README.md`, all `app.brand.*` keys in `locales/*.json`. If a commercial product name is needed later, it is introduced as an additional skin without changing the open-source default.

## Consequences

- Single-source brand strings live in i18n; do not hard-code product name in components.
- README references to "DRS" are rewritten; `web-prototype/README.md`'s "Design decisions (frozen)" section moves into ADRs (this one and ADR-0008 onward).
