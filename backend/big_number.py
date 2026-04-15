from __future__ import annotations

from typing import Union


NumberLike = Union[int, str]

_BASE_UNITS: list[tuple[int, str]] = [
    (4, "万"),
    (8, "亿"),
    (12, "兆"),
    (16, "京"),
    (20, "垓"),
    (24, "秭"),
    (28, "穰"),
    (32, "沟"),
    (36, "涧"),
    (40, "正"),
    (44, "载"),
    (48, "极"),
]

_HUMAN_UNITS: list[tuple[int, str]] = []
for repeat in range(0, 7):
    suffix = "极" * repeat
    for exponent, label in _BASE_UNITS:
        total_exponent = exponent + repeat * 48
        if total_exponent < 8 or total_exponent > 300:
            continue
        _HUMAN_UNITS.append((total_exponent, label + suffix))
_HUMAN_UNITS.sort(key=lambda item: item[0])


def _strip_leading_zeros(digits: str) -> str:
    stripped = digits.lstrip("0")
    return stripped or "0"


def _normalize_scientific_numeric_string(text: str) -> str:
    sign = ""
    if text[:1] in {"+", "-"}:
        sign = "-" if text[0] == "-" else ""
        text = text[1:]

    mantissa, exponent_text = text.lower().split("e", 1)
    exponent = int(exponent_text)

    if "." in mantissa:
        integer_part, fractional_part = mantissa.split(".", 1)
    else:
        integer_part, fractional_part = mantissa, ""

    integer_part = integer_part or "0"
    digits = _strip_leading_zeros(f"{integer_part}{fractional_part}")
    if digits == "0":
        return "0"

    scale = exponent - len(fractional_part)
    if scale >= 0:
        normalized = digits + ("0" * scale)
    else:
        cutoff = len(digits) + scale
        if cutoff <= 0:
            return "0"
        normalized = digits[:cutoff]

    normalized = _strip_leading_zeros(normalized)
    if normalized == "0":
        return "0"
    return f"-{normalized}" if sign else normalized


def normalize_numeric_string(value: NumberLike) -> str:
    if isinstance(value, int):
        return str(value)

    text = str(value or "").strip()
    if not text:
        return "0"

    if "e" in text.lower():
        return _normalize_scientific_numeric_string(text)

    if "." in text:
        sign = ""
        if text[:1] in {"+", "-"}:
            sign = "-" if text[0] == "-" else ""
            text = text[1:]
        integer_part, fractional_part = text.split(".", 1)
        integer_part = _strip_leading_zeros(integer_part or "0")
        if integer_part == "0":
            return "0"
        return f"-{integer_part}" if sign else integer_part

    return str(int(text))


def to_plain_numeric_string(value: NumberLike) -> str:
    return normalize_numeric_string(value)


def _format_fixed_from_plain_numeric_string(value: str, exponent: int, decimals: int) -> str:
    number = int(value)
    scale = 10 ** exponent
    scaled = number * (10 ** decimals)
    quotient, remainder = divmod(scaled, scale)
    if remainder * 2 >= scale:
        quotient += 1

    text = str(quotient)
    if decimals <= 0:
        return text
    if len(text) <= decimals:
        text = text.zfill(decimals + 1)
    return f"{text[:-decimals]}.{text[-decimals:]}"


def format_readable_numeric_string(
        value: NumberLike,
        *,
        human_decimals: int = 2,
        scientific_decimals: int = 5,
) -> str:
    normalized = normalize_numeric_string(value)
    negative = normalized.startswith("-")
    digits = normalized[1:] if negative else normalized
    if digits == "0":
        return "0"
    if len(digits) - 1 < 8:
        return normalized

    exponent = len(digits) - 1
    sign = "-" if negative else ""

    if exponent >= 304:
        mantissa = _format_fixed_from_plain_numeric_string(digits, exponent, scientific_decimals)
        return f"{sign}{mantissa}e{exponent}"

    unit_exponent, unit_label = max(
        (item for item in _HUMAN_UNITS if item[0] <= exponent),
        key=lambda item: item[0],
    )
    coeff = _format_fixed_from_plain_numeric_string(digits, unit_exponent, human_decimals)
    return f"{sign}{coeff}{unit_label}"


def compare_numeric_strings(a: NumberLike, b: NumberLike) -> int:
    left = int(normalize_numeric_string(a))
    right = int(normalize_numeric_string(b))
    if left < right:
        return -1
    if left > right:
        return 1
    return 0


def add_numeric_strings(a: NumberLike, b: NumberLike) -> str:
    return str(int(normalize_numeric_string(a)) + int(normalize_numeric_string(b)))


def multiply_numeric_strings(a: NumberLike, b: NumberLike) -> str:
    return str(int(normalize_numeric_string(a)) * int(normalize_numeric_string(b)))


def divide_numeric_strings(
        a: NumberLike,
        b: NumberLike,
        *,
        decimals: int = 5,
        trim_trailing_zeros: bool = True,
) -> str:
    left = int(normalize_numeric_string(a))
    right = int(normalize_numeric_string(b))
    if right == 0:
        raise ZeroDivisionError("Cannot divide by zero")

    negative = (left < 0) != (right < 0)
    left = abs(left)
    right = abs(right)
    scale = 10 ** max(0, decimals)
    scaled = left * scale
    quotient, remainder = divmod(scaled, right)
    if remainder * 2 >= right:
        quotient += 1

    text = str(quotient)
    if decimals > 0:
        if len(text) <= decimals:
            text = text.zfill(decimals + 1)
        text = f"{text[:-decimals]}.{text[-decimals:]}"
        if trim_trailing_zeros:
            text = text.rstrip("0").rstrip(".")
    if negative and text != "0":
        text = f"-{text}"
    return text
