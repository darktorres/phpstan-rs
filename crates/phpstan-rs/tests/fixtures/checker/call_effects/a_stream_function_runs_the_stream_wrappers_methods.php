<?php
namespace CallEffects\StreamWrapper;

use function PHPStan\Testing\assertType;

final class RecordingWrapper
{
    public static int $opens = 0;

    /** @var resource|null */
    public $context;

    public function stream_open(string $path, string $mode, int $options, ?string &$openedPath): bool
    {
        self::$opens++;
        return true;
    }

    public function stream_read(int $count): string
    {
        return '';
    }

    public function stream_eof(): bool
    {
        return true;
    }

    /** @return array<int|string, int> */
    public function stream_stat(): array
    {
        return [];
    }
}

function check(): void
{
    stream_wrapper_register('recording', RecordingWrapper::class);
    RecordingWrapper::$opens = 0;
    echo file_get_contents('recording://anything');
    assertType('int', RecordingWrapper::$opens);
}

check();
