import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ErrorScreen, RootErrorBoundary } from "./RootErrorBoundary";

describe("RootErrorBoundary", () => {
  it("switches to the error screen instead of unmounting the tree", () => {
    const next = RootErrorBoundary.getDerivedStateFromError(new Error("boom"));
    expect(next.error?.message).toBe("boom");
  });

  it("renders the report with the message, stack and component stack", () => {
    const error = new Error("Cannot read properties of undefined (reading 'length')");
    error.stack = "Error: length\n    at NavRail (NavRail.tsx:45:3)";
    const markup = renderToStaticMarkup(
      <ErrorScreen error={error} componentStack={"\n    at NavRail\n    at App"} />,
    );
    expect(markup).toContain("Интерфейс упал");
    expect(markup).toContain("reading &#x27;length&#x27;");
    expect(markup).toContain("at NavRail (NavRail.tsx:45:3)");
    expect(markup).toContain("Стек компонентов:");
    // Экран не должен зависеть от таблицы стилей приложения: только инлайн.
    expect(markup).not.toContain("class=");
  });
});
