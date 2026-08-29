// @vitest-environment jsdom

import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useSmoothStream } from "./useSmoothStream";

afterEach(() => cleanup());

function Probe({ target, active }: { target: string; active: boolean }) {
  const value = useSmoothStream(target, active);
  return <span data-testid="out">{value}</span>;
}

describe("useSmoothStream", () => {
  it("returns the full target when inactive", () => {
    const { getByTestId } = render(<Probe target="complete" active={false} />);
    expect(getByTestId("out").textContent).toBe("complete");
  });

  it("starts from the current target while streaming", () => {
    const { getByTestId } = render(<Probe target="abc" active />);
    expect(getByTestId("out").textContent).toBe("abc");
  });
});
