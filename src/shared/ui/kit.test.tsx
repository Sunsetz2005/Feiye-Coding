// @vitest-environment jsdom

import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  SHARED_UI_MODULE,
  Banner,
  Button,
  Chip,
  EmptyState,
  IconButton,
  SearchField,
  SegmentedControl,
  Skeleton,
} from "./index";
import {
  Button as CompatButton,
  COMPAT_BUTTON_MODULE,
} from "@/components/ui/button";

afterEach(() => cleanup());

describe("shared/ui kit", () => {
  it("Banner exposes status and optional action", () => {
    const onPress = vi.fn();
    const { getByRole } = render(
      <Banner
        tone="warning"
        action={{ label: "Choose folder", onPress }}
      >
        No project selected
      </Banner>,
    );
    expect(getByRole("status").textContent).toContain("No project selected");
    getByRole("button", { name: "Choose folder" }).click();
    expect(onPress).toHaveBeenCalledOnce();
  });

  it("Banner defaults to info", () => {
    const { getByRole } = render(<Banner>Hello</Banner>);
    expect(getByRole("status").getAttribute("data-tone")).toBe("info");
  });

  it("Banner supports danger tone", () => {
    const { getByRole } = render(<Banner tone="danger">Boom</Banner>);
    expect(getByRole("status").getAttribute("data-tone")).toBe("danger");
  });

  it("Banner can be dismissed", () => {
    const onDismiss = vi.fn();
    const { getByRole, rerender } = render(
      <Banner onDismiss={onDismiss} dismissLabel="Dismiss notice">
        Notice
      </Banner>,
    );
    getByRole("button", { name: "Dismiss notice" }).click();
    expect(onDismiss).toHaveBeenCalledOnce();
    rerender(
      <Banner onDismiss={onDismiss}>Notice</Banner>,
    );
    expect(getByRole("button", { name: "Dismiss" })).toBeTruthy();
  });

  it("Button supports size and link variants", () => {
    const { getByRole } = render(
      <Button size="lg" variant="link">
        Docs
      </Button>,
    );
    expect(getByRole("button", { name: "Docs" })).toBeTruthy();
  });

  it("Button can render as a child link", () => {
    const { getByRole } = render(
      <Button asChild>
        <a href="#docs">Open docs</a>
      </Button>,
    );
    expect(getByRole("link", { name: "Open docs" }).getAttribute("href")).toBe(
      "#docs",
    );
  });

  it("Button sets aria-busy while loading and blocks clicks", () => {
    const onClick = vi.fn();
    const { getByRole } = render(
      <Button loading onClick={onClick}>
        Connect
      </Button>,
    );
    const btn = getByRole("button", { name: "Connect" });
    expect(btn.getAttribute("aria-busy")).toBe("true");
    expect(btn.hasAttribute("disabled")).toBe(true);
    btn.click();
    expect(onClick).not.toHaveBeenCalled();
  });

  it("IconButton requires an accessible name", () => {
    const { getByRole } = render(
      <IconButton aria-label="Plugins">P</IconButton>,
    );
    expect(getByRole("button", { name: "Plugins" })).toBeTruthy();
  });

  it("EmptyState is not icon-only", () => {
    const { getByRole, rerender } = render(
      <EmptyState title="No personal plugins" description="Use the public tab." />,
    );
    const status = getByRole("status");
    expect(status.textContent).toContain("No personal plugins");
    expect(status.textContent).toContain("Use the public tab.");
    rerender(<EmptyState title="No personal plugins" />);
    expect(getByRole("status").textContent).toContain("No personal plugins");
    rerender(
      <EmptyState title="No personal plugins" action={<button type="button">Go</button>} />,
    );
    expect(getByRole("button", { name: "Go" })).toBeTruthy();
  });

  it("SearchField associates the visible label", () => {
    const { getByLabelText } = render(
      <SearchField label="Search plugins" value="" onChange={() => {}} />,
    );
    expect(getByLabelText("Search plugins")).toBeTruthy();
  });

  it("SearchField can hide the label visually", () => {
    const onChange = vi.fn();
    const { getByLabelText } = render(
      <SearchField
        label="Hidden search"
        visuallyHideLabel
        value="q"
        placeholder="Find"
        className="extra"
        onChange={onChange}
      />,
    );
    const input = getByLabelText("Hidden search") as HTMLInputElement;
    expect(input.placeholder).toBe("Find");
    fireEvent.change(input, { target: { value: "next" } });
    expect(onChange).toHaveBeenCalledWith("next");
  });

  it("SegmentedControl moves with arrow keys", () => {
    const onChange = vi.fn();
    const { getByRole } = render(
      <SegmentedControl
        ariaLabel="Audience"
        value="public"
        onChange={onChange}
        options={[
          { value: "public", label: "Public" },
          { value: "personal", label: "Personal" },
        ]}
      />,
    );
    const list = getByRole("tablist", { name: "Audience" });
    list.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
    );
    expect(onChange).toHaveBeenCalledWith("personal");
    onChange.mockClear();
    list.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }),
    );
    expect(onChange).toHaveBeenCalledWith("personal");
    onChange.mockClear();
    list.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    );
    list.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }),
    );
    list.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(onChange).toHaveBeenCalled();
  });

  it("SegmentedControl ignores keys when value is unknown", () => {
    const onChange = vi.fn();
    const { getByRole } = render(
      <div>
        <span id="aud">Audience</span>
        <SegmentedControl
          labelledBy="aud"
          value={"other" as "public"}
          onChange={onChange}
          options={[
            { value: "public", label: "Public" },
            { value: "personal", label: "Personal" },
          ]}
        />
      </div>,
    );
    getByRole("tablist").dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
    );
    expect(onChange).not.toHaveBeenCalled();
  });

  it("Chip marks the current selection", () => {
    const onPress = vi.fn();
    const { getByRole } = render(
      <Chip current leading="📁" onPress={onPress} ariaLabel="Project">
        Sunsetz
      </Chip>,
    );
    const chip = getByRole("button", { name: "Project" });
    expect(chip.getAttribute("aria-current")).toBe("true");
    chip.click();
    expect(onPress).toHaveBeenCalledOnce();
  });

  it("Chip can be disabled", () => {
    const { getByRole } = render(
      <Chip disabled ariaLabel="Locked">
        Locked
      </Chip>,
    );
    expect(getByRole("button", { name: "Locked" }).hasAttribute("disabled")).toBe(
      true,
    );
  });

  it("re-exports the compatibility Button", () => {
    expect(CompatButton).toBe(Button);
    expect(SHARED_UI_MODULE).toBe("shared-ui");
    expect(COMPAT_BUTTON_MODULE).toBe("components-ui-button");
  });

  it("Skeleton is hidden from assistive tech", () => {
    const { container } = render(
      <Skeleton lines={0} media className="pad" />,
    );
    expect(container.firstElementChild?.getAttribute("aria-hidden")).toBe(
      "true",
    );
  });
});
