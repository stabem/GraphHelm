import { DisconnectedError, RuntimeError } from "./client";

/** The Runtime's own words for a refusal it sent; anything else reads as `fallback`. */
export function messageOf(error: unknown, fallback: string): string {
  if (error instanceof RuntimeError || error instanceof DisconnectedError) return error.message;
  return fallback;
}
