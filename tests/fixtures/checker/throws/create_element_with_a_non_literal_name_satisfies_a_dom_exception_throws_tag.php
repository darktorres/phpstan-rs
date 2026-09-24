<?php
            /** @throws \DOMException */
            function f(\DOMDocument $doc, string $name): void {
                $doc->createElement($name);
            }
        