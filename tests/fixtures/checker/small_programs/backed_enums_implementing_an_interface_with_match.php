<?php

interface HasLabel
{
    public function label(): string;
}

enum Suit: string implements HasLabel
{
    case Hearts = 'H';
    case Diamonds = 'D';
    case Clubs = 'C';
    case Spades = 'S';

    public function label(): string
    {
        return match ($this) {
            Suit::Hearts => 'Hearts',
            Suit::Diamonds => 'Diamonds',
            Suit::Clubs => 'Clubs',
            Suit::Spades => 'Spades',
        };
    }
}

enum Direction
{
    case North;
    case South;
    case East;
    case West;
}

function needsHasLabel(HasLabel $h): string
{
    return $h->label();
}

echo needsHasLabel(Suit::Hearts);
