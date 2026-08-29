import type { ConnectorCatalogEntry } from "@/lib/connectorCatalog";

/** Official product marks bundled from each vendor site. Keys match catalog ids. */
const logoModules = import.meta.glob("../assets/connectors/*.{svg,png}", {
  eager: true,
  import: "default",
}) as Record<string, string>;

export function connectorLogoSrc(id: string): string | undefined {
  return (
    logoModules[`../assets/connectors/${id}.svg`] ??
    logoModules[`../assets/connectors/${id}.png`]
  );
}

export function ConnectorLogo({
  connector,
  size = 36,
}: {
  connector: Pick<ConnectorCatalogEntry, "id" | "color" | "glyph" | "nameKey">;
  size?: number;
}) {
  const src = connectorLogoSrc(connector.id);
  const bleed = connector.id === "granola";
  return (
    <span
      className={
        `connector-logo connector-logo--${connector.id}` +
        (bleed ? " connector-logo--bleed" : "")
      }
      style={{
        width: size,
        height: size,
        fontSize: size > 40 ? 18 : 13,
        ...(src ? undefined : { background: connector.color }),
      }}
      aria-hidden
    >
      {src ? (
        <img
          className="connector-logo__img"
          src={src}
          alt=""
          draggable={false}
        />
      ) : (
        connector.glyph
      )}
    </span>
  );
}
