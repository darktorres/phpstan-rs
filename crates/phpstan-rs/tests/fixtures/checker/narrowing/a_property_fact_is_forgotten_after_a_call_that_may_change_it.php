<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

final class Anomalies
{
    /** @var list<string> */
    private array $found = [];

    private ?string $name = null;

    public function add(string $anomaly): void
    {
        $this->found[] = $anomaly;
    }

    public function name(): ?string
    {
        return $this->name;
    }

    public function forget(): void
    {
        $this->name = null;
    }

    public function check(): void
    {
        $this->found = [];
        $this->collect();
        assertType('list<string>', $this->found);
        foreach ($this->found as $anomaly) {
        }
    }

    public function describe(string $name): void
    {
        $this->name = $name;
        $this->name();
        assertType('string', $this->name);
        self::reset($this);
        $this->forget();
        assertType('string|null', $this->name);
    }

    private static function reset(self $anomalies): void
    {
    }

    private function collect(): void
    {
        $this->add('missing file');
    }
}

final class Mail
{
    public ?string $subject = null;

    public ?string $theme = null;
}

final class Envelope
{
    public function __construct(private Mail $mail)
    {
    }
}

function resolveTheme(Mail $mail): string
{
    if ($mail->theme === null || !in_array($mail->theme, ['clear', 'dark'], true)) {
        echo 'default theme';

        return 'clear';
    }
    if ($mail->theme === '') {
        throw new InvalidArgumentException('empty');
    }

    return $mail->theme;
}

function clearSubject(Mail $mail): void
{
    $mail->subject = null;
}

function send(Mail $mail): void
{
    if ($mail->subject === null) {
        return;
    }
    resolveTheme($mail);
    new Envelope($mail);
    assertType('string', $mail->subject);
    clearSubject($mail);
    assertType('string|null', $mail->subject);
}
