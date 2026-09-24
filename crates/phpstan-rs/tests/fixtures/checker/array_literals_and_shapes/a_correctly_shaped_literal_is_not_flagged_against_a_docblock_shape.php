<?php
            /** @param array{0: int, 1: string} $pair */
            function needsPair(array $pair): void {}
            function test(): void { needsPair([1, "a"]); }
        