import { describe, expect, it, vi } from "vitest";
import { createFloatingSurfaceOwner } from "./FloatingSurfaceProvider";

describe("floating surface ownership", () => {
  it("hands ownership to the newly opened surface and closes the old one", () => {
    const owner = createFloatingSurfaceOwner();
    const closeModel = vi.fn();
    const closeContext = vi.fn();

    owner.claim("model", closeModel);
    expect(owner.activeId).toBe("model");

    owner.claim("context", closeContext);
    expect(closeModel).toHaveBeenCalledOnce();
    expect(closeModel).toHaveBeenCalledWith("ownership-handoff");
    expect(closeContext).not.toHaveBeenCalled();
    expect(owner.activeId).toBe("context");
  });

  it("ignores a stale release from the surface that just lost ownership", () => {
    const owner = createFloatingSurfaceOwner();

    owner.claim("plus", () => owner.release("plus"));
    owner.claim("project", () => undefined);

    owner.release("plus");
    expect(owner.activeId).toBe("project");
  });

  it("updates the callback when the current owner reclaims without closing it", () => {
    const owner = createFloatingSurfaceOwner();
    const oldClose = vi.fn();
    const newClose = vi.fn();

    owner.claim("access", oldClose);
    owner.claim("access", newClose);

    expect(oldClose).not.toHaveBeenCalled();
    expect(owner.activeId).toBe("access");

    owner.claim("model", () => undefined);
    expect(newClose).toHaveBeenCalledOnce();
  });
});
