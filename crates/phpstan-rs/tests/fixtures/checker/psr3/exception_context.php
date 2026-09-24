<?php
namespace Psr\Log {
    interface LoggerInterface {
        public function log($level, string|\Stringable $message, array $context = []): void;
        public function error(string|\Stringable $message, array $context = []): void;
        public function info(string|\Stringable $message, array $context = []): void;
    }
}
namespace App {
use Psr\Log\LoggerInterface;
function maybe_throws(): void {}
function f(LoggerInterface $logger): void {
    try {
        maybe_throws();
    } catch (\RuntimeException $e) {
        $logger->error('failed');
        $logger->error('failed', ['exception' => $e]);
        $logger->error('failed', ['other' => 1]);
    }
    $logger->error('outside');
}
}
