<?php
namespace CallEffects\ChainedReceivers;

use function PHPStan\Testing\assertType;

final class Flag
{
    public static ?int $value = null;
}

final class Writer
{
    public function run(): void
    {
        Flag::$value = 5;
    }
}

final class Reader
{
    public function read(): int
    {
        return 1;
    }
}

final class Factory
{
    public function writer(): Writer
    {
        return new Writer();
    }

    public function reader(): Reader
    {
        return new Reader();
    }
}

function throughAWriter(Factory $factory): void
{
    $factory->writer()->run();
}

function throughAReader(Factory $factory): void
{
    $factory->reader()->read();
}

function onANewWriter(): void
{
    (new Writer())->run();
}

function onANewReader(): void
{
    (new Reader())->read();
}

function theWriterChainIsForgotten(Factory $factory): void
{
    Flag::$value = null;
    throughAWriter($factory);
    assertType('int|null', Flag::$value);
}

function theNewWriterIsForgotten(): void
{
    Flag::$value = null;
    onANewWriter();
    assertType('int|null', Flag::$value);
}

function theReaderChainKeepsTheFact(Factory $factory): void
{
    Flag::$value = null;
    throughAReader($factory);
    assertType('null', Flag::$value);
}

function theNewReaderKeepsTheFact(): void
{
    Flag::$value = null;
    onANewReader();
    assertType('null', Flag::$value);
}
