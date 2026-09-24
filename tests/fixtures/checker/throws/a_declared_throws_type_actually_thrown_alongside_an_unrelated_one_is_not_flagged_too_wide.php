<?php
            class NeedleException extends \RuntimeException {}
            class UnrelatedException extends \RuntimeException {}
            /** @throws UnrelatedException */
            function callee(): void {
                throw new UnrelatedException('unrelated');
            }
            /** @throws NeedleException */
            function f(bool $b): void {
                if ($b) {
                    callee();
                }
                throw new NeedleException('real');
            }
        