<?php
            namespace App {
            /** @param resource $handle */
            function useHandle($handle): void {}

            } namespace {
                /** @param resource $h */
                function passthrough($h): void { \App\useHandle($h); }
            }
        