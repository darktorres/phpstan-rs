<?php

namespace Psr\Http\Message {
    interface MessageInterface
    {
        /** @return string[] */
        public function getHeader(string $name): array;
    }

    interface ServerRequestInterface extends MessageInterface
    {
        /** @return array<string, mixed> */
        public function getQueryParams(): array;
    }
}

namespace App {
    use Psr\Http\Message\ServerRequestInterface;
    use function PhpstanRs\Testing\assertFlow;

    function param(string $name): string
    {
        return $name;
    }

    function sources(string $path, \PDO $db, ServerRequestInterface $request): void
    {
        assertFlow('env:file_get_contents()', file_get_contents($path));
        assertFlow('env:filesize()', filesize($path));
        assertFlow('env:getenv()', getenv('PAGE'));
        $statement = $db->query('SELECT 1');
        if ($statement !== false) {
            assertFlow('env:PDOStatement::fetchColumn()', $statement->fetchColumn());
        }
        assertFlow('env:Psr\Http\Message\ServerRequestInterface::getQueryParams()', $request->getQueryParams());
        assertFlow('env:Psr\Http\Message\MessageInterface::getHeader()', $request->getHeader('X-Page'));
        assertFlow('env:App\param()', param('page'));
        assertFlow('', strlen($path));
    }
}

namespace {
    use function PhpstanRs\Testing\assertFlow;

    assertFlow('env:$argv', $argv);
}
