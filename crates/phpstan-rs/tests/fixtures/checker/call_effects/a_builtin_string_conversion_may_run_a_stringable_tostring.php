<?php

final class ConvertedBox
{
    public ?string $value = null;
}

final class ConvertedWriter
{
    public function __construct(private ConvertedBox $box)
    {
    }

    public function __toString(): string
    {
        $this->box->value = null;

        return 'written';
    }
}

function convertedValueAfterASprintfOfAStringable(ConvertedBox $box, ?string $candidate, ConvertedWriter $writer): string
{
    if ($candidate !== null && $box->value !== null) {
        $text = sprintf('%s', $writer);
        assertType('string|null', $box->value);

        return $text;
    }

    return '';
}
