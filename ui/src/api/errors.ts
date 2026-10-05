import type { ApiError, ErrorCode } from "../types/generated";

/** True when `value` is the `ApiError` shape every command rejects with. */
export function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as ApiError).code === "string" &&
    typeof (value as ApiError).message === "string"
  );
}

/** Converts anything thrown into an `ApiError`, so screens handle one shape. */
export function toApiError(value: unknown): ApiError {
  if (isApiError(value)) return value;
  const message = value instanceof Error ? value.message : String(value);
  return { code: "internal", message };
}

/** Creates an `ApiError` (used by the mock backend). */
export function apiError(code: ErrorCode, message: string): ApiError {
  return { code, message };
}
