<?php
            namespace Amp;
            interface ResourceStream { public function unreference(): void; }
            interface Socket { public function read(): ?string; }
            function f(?Socket $socket): void {
                if ($socket instanceof ResourceStream) {
                    $socket->unreference();
                }
                $socket?->read();
            }
        